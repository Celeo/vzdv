fetch("/facility/roster/stats/data")
  .then((response) => response.json())
  .then((data) => {
    const chartRating = echarts.init(document.getElementById("ratings"), "dark");
    const optionRating = {
      series: [
        {
          type: "pie",
          data: data.by_rating,
          label: {
            show: true,
            formatter: "{b}: {c}"
          }
        },
      ],
    };
    chartRating.setOption(optionRating);

    const chartCert = echarts.init(document.getElementById("certs"), "dark");
    const optionCert = {
      xAxis: {
        data: data.certs,
      },
      yAxis: {},
      series: [
        {
          type: "bar",
          data: data.by_cert,
          label: {
            show: true
          }
        },
      ],
    };
    chartCert.setOption(optionCert);
  })
  .catch((err) => console.error(err));
